use std::{env, path::Path, sync::Arc};

use image::{ImageBuffer, Rgb};

use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;
const OW_L2_COLS: u32 = 64;
const OW_L2_ROWS: u32 = 64;

fn main() {
    let args: Vec<String> = env::args().collect();
    let rom_path = args
        .iter()
        .find_map(|a| a.strip_prefix("--rom="))
        .map(Path::new)
        .or_else(|| {
            args.iter()
                .skip(1)
                .find(|a| !a.starts_with("--"))
                .map(|a| Path::new(a))
        })
        .unwrap_or_else(|| Path::new("smw.smc"));
    let submap = args.iter().find_map(|a| a.strip_prefix("--submap=")).and_then(|s| s.parse::<u8>().ok()).unwrap_or(3);
    let output = args
        .iter()
        .find_map(|a| a.strip_prefix("--out="))
        .unwrap_or("ow_render.png");
    let full = args.iter().any(|a| a == "--full");
    let activate_events = !args.iter().any(|a| a == "--no-events");

    let raw = std::fs::read(rom_path).expect("cannot read smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };

    if args.iter().any(|a| a == "--dump-levels") {
        dump_levels(&rom_bytes);
        return;
    }

    if args.iter().any(|a| a == "--dump-events") {
        dump_events(&rom_bytes);
        return;
    }

    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

    if activate_events {
        activate_all_overworld_events(&mut cpu);
    }
    smwe_emu::emu::load_overworld(&mut cpu, submap);

    if args.iter().any(|a| a == "--dump-l1-atlas") {
        dump_l1_atlas(&mut cpu, output);
        return;
    }

    if let Some(spec) = args.iter().find_map(|a| a.strip_prefix("--dump-l1=")) {
        let parts: Vec<u32> = spec.split(',').map(|s| s.parse().unwrap()).collect();
        let (col0, row0, col1, row1) = (parts[0], parts[1], parts[2], parts[3]);
        for row in row0..=row1 {
            for col in col0..=col1 {
                let addr = tilemap_vram_addr(VRAM_L1_TILEMAP_BASE, col, row);
                let t0 = cpu.mem.vram[addr] as u16;
                let t1 = cpu.mem.vram[addr + 1] as u16;
                let tile_id = t0 | ((t1 & 3) << 8);
                print!("{tile_id:03X} ");
            }
            println!();
        }
        return;
    }

    let l2_scroll_x = i16::from_le_bytes(cpu.mem.load_u16(0x001E).to_le_bytes()) as i32;
    let l2_scroll_y = i16::from_le_bytes(cpu.mem.load_u16(0x0020).to_le_bytes()) as i32;

    let (w, h) = if full { (1024u32, 512u32) } else { (512u32, 512u32) };
    let mut pixels = vec![0u8; (w * h * 3) as usize];
    if full {
        render_bg_full(&cpu.mem.vram, VRAM_L2_TILEMAP_BASE, w, &cpu.mem.cgram, &mut pixels);
        render_bg_full(&cpu.mem.vram, VRAM_L1_TILEMAP_BASE, w, &cpu.mem.cgram, &mut pixels);
    } else {
        render_bg(&cpu.mem.vram, VRAM_L2_TILEMAP_BASE, l2_scroll_x, l2_scroll_y, 512, &cpu.mem.cgram, &mut pixels);
        render_bg(&cpu.mem.vram, VRAM_L1_TILEMAP_BASE, l2_scroll_x, l2_scroll_y, 512, &cpu.mem.cgram, &mut pixels);
    }

    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, pixels).expect("image buffer");
    img.save(output).expect("save png");
    println!("wrote {output}");
}

/// Dump all 256 Map16Common OW L1 tile IDs as a labeled 16x16 grid atlas, using
/// the exact same tile resolution logic as `world_editor::ow_tile_picker::OwL1TilePicker`.
fn dump_l1_atlas(cpu: &mut Cpu, output: &str) {
    const COLS: usize = 16;
    const ROWS: usize = 16;
    const TILE_PX: usize = 16;
    const LABEL_PX: usize = 8;
    const CELL_PX: usize = TILE_PX + LABEL_PX;
    let w = (COLS * CELL_PX) as u32;
    let h = (ROWS * CELL_PX) as u32;
    let mut pixels = vec![0u8; (w * h * 3) as usize];

    let ptr_base = 0x7E0FBEu32;
    let char_bank = 0x05_0000u32;

    for tile_id in 0..256u32 {
        let col = tile_id as usize % COLS;
        let row = tile_id as usize / COLS;
        let x0 = (col * CELL_PX) as u32;
        let y0 = (row * CELL_PX) as u32 + LABEL_PX as u32;

        let char_ptr = cpu.mem.load_u16(ptr_base + tile_id * 2) as u32;
        let gfx_addr = char_bank | char_ptr;
        let sub_tiles =
            [cpu.mem.load_u16(gfx_addr), cpu.mem.load_u16(gfx_addr + 2), cpu.mem.load_u16(gfx_addr + 4), cpu.mem.load_u16(gfx_addr + 6)];

        let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
        for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
            let t = sub_tiles[sub_i];
            let tile_num = (t & 0x3FF) as usize;
            let pal = ((t >> 10) & 0x7) as usize;
            let flip_x = (t & 0x4000) != 0;
            let flip_y = (t & 0x8000) != 0;
            render_l1_sub_tile_2x(&cpu.mem.vram, &cpu.mem.cgram, tile_num, pal, flip_x, flip_y, x0 + sx * 2, y0 + sy * 2, w, &mut pixels);
        }
        draw_hex_label(tile_id as u8, x0, y0 - LABEL_PX as u32, w, &mut pixels);
    }

    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(w, h, pixels).expect("image buffer");
    img.save(output).expect("save png");
    println!("wrote {output}");
}

/// Like `render_tile` but draws each source pixel as a 2x2 block (sub-tiles are 8px, doubled to 16px).
#[allow(clippy::too_many_arguments)]
fn render_l1_sub_tile_2x(
    vram: &[u8], cgram: &[u8], tile_id: usize, palette: usize, flip_x: bool, flip_y: bool, x0: u32, y0: u32,
    width: u32, pixels: &mut [u8],
) {
    let tile_base = tile_id * 32;
    for ty in 0..8u32 {
        for tx in 0..8u32 {
            let px = if flip_x { 7 - tx } else { tx };
            let py = if flip_y { 7 - ty } else { ty };
            let row_off = tile_base + (py as usize) * 2;
            if row_off + 17 >= vram.len() {
                continue;
            }
            let b0 = vram[row_off];
            let b1 = vram[row_off + 1];
            let b2 = vram[row_off + 16];
            let b3 = vram[row_off + 17];
            let bit = 7 - px as usize;
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;
            let rgb = if color_idx == 0 { [24, 24, 24] } else { read_color(cgram, palette * 16 + color_idx) };
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    let px_x = x0 + tx * 2 + dx;
                    let px_y = y0 + ty * 2 + dy;
                    let off = ((px_y * width + px_x) * 3) as usize;
                    if off + 2 < pixels.len() {
                        pixels[off] = rgb[0];
                        pixels[off + 1] = rgb[1];
                        pixels[off + 2] = rgb[2];
                    }
                }
            }
        }
    }
}

/// Draw a 2-digit hex label using a tiny fixed 3x5 bitmap font, for atlas debugging.
fn draw_hex_label(val: u8, x0: u32, y0: u32, width: u32, pixels: &mut [u8]) {
    const FONT: [(char, [u8; 5]); 16] = [
        ('0', [0b111, 0b101, 0b101, 0b101, 0b111]),
        ('1', [0b010, 0b110, 0b010, 0b010, 0b111]),
        ('2', [0b111, 0b001, 0b111, 0b100, 0b111]),
        ('3', [0b111, 0b001, 0b111, 0b001, 0b111]),
        ('4', [0b101, 0b101, 0b111, 0b001, 0b001]),
        ('5', [0b111, 0b100, 0b111, 0b001, 0b111]),
        ('6', [0b111, 0b100, 0b111, 0b101, 0b111]),
        ('7', [0b111, 0b001, 0b001, 0b001, 0b001]),
        ('8', [0b111, 0b101, 0b111, 0b101, 0b111]),
        ('9', [0b111, 0b101, 0b111, 0b001, 0b111]),
        ('A', [0b111, 0b101, 0b111, 0b101, 0b101]),
        ('B', [0b110, 0b101, 0b110, 0b101, 0b110]),
        ('C', [0b111, 0b100, 0b100, 0b100, 0b111]),
        ('D', [0b110, 0b101, 0b101, 0b101, 0b110]),
        ('E', [0b111, 0b100, 0b111, 0b100, 0b111]),
        ('F', [0b111, 0b100, 0b111, 0b100, 0b100]),
    ];
    let hex_digit = |c: char| FONT.iter().find(|(fc, _)| *fc == c).map(|(_, bits)| *bits).unwrap();
    let s = format!("{val:02X}");
    for (ci, ch) in s.chars().enumerate() {
        let bits = hex_digit(ch);
        for row in 0..5u32 {
            for col in 0..3u32 {
                if (bits[row as usize] >> (2 - col)) & 1 != 0 {
                    let px_x = x0 + ci as u32 * 4 + col;
                    let px_y = y0 + row;
                    let off = ((px_y * width + px_x) * 3) as usize;
                    if off + 2 < pixels.len() {
                        pixels[off] = 255;
                        pixels[off + 1] = 255;
                        pixels[off + 2] = 0;
                    }
                }
            }
        }
    }
}

/// Print every overworld level tile's (col, row, translevel, level_number),
/// to sanity-check `smwe_rom::overworld::OverworldData::level_number_at`
/// against known vanilla level numbering (e.g. Yoshi's Island 1 = 0x000).
fn dump_levels(rom_bytes: &[u8]) {
    let rom = smwe_rom::snes_utils::rom::Rom::new(rom_bytes.to_vec()).expect("rom parse");
    let ow = smwe_rom::overworld::OverworldData::parse(&rom).expect("overworld parse");
    for row in 0..smwe_rom::overworld::OW_HEIGHT_TILES {
        for col in 0..smwe_rom::overworld::OW_WIDTH_TILES {
            if let Some(level_num) = ow.level_number_at(col, row) {
                let translevel = ow.translevel_at(col, row).unwrap();
                let tile_id = ow.tile_at(col, row);
                println!("col={col:2} row={row:2} tile_id={tile_id:#04X} translevel={translevel:#04X} level_number={level_num:#04X}");
            }
        }
    }
}

/// Parse OverworldEvents from the real ROM and print the reveal-tile tables
/// plus a sample of nonzero tile offsets, to sanity-check the transcription in
/// smwe_rom::overworld against the actual ROM bytes.
fn dump_events(rom_bytes: &[u8]) {
    let rom = smwe_rom::snes_utils::rom::Rom::new(rom_bytes.to_vec()).expect("rom parse");
    let events = smwe_rom::overworld::OverworldEvents::parse(&rom).expect("events parse");
    println!("reveal_before: {:02X?}", events.reveal_before);
    println!("reveal_after:  {:02X?}", events.reveal_after);
    println!("nonzero tile_offsets:");
    for (i, &off) in events.tile_offsets.iter().enumerate() {
        if off != 0 {
            println!("  event {i:3} -> offset {off:#06X}");
        }
    }

    let ow = smwe_rom::overworld::OverworldData::parse(&rom).expect("overworld parse");
    let mut tiles = ow.layer1_tiles.clone();
    let mut active = vec![false; smwe_rom::overworld::OW_EVENT_COUNT];
    for a in active.iter_mut() {
        *a = true;
    }
    events.apply(&mut tiles, &active);
    let mut changed = 0;
    for (i, (&before, &after)) in ow.layer1_tiles.iter().zip(tiles.iter()).enumerate() {
        if before != after {
            changed += 1;
            if changed <= 20 {
                println!("  tile[{i:#06X}] {before:#04X} -> {after:#04X}");
            }
        }
    }
    println!("total tiles changed by applying all events: {changed}");
}

fn activate_all_overworld_events(cpu: &mut Cpu) {
    for addr in 0x1F02u32..=0x1F60 {
        cpu.mem.store_u8(addr, 0xFF);
    }
}

fn tilemap_vram_addr(base: usize, col: u32, row: u32) -> usize {
    let quadrant = ((row / 32) * 2) + (col / 32);
    let sub_row = row % 32;
    let sub_col = col % 32;
    let quadrant_offset = quadrant * 32 * 32 * 2;
    let idx = quadrant_offset + ((sub_row * 32 + sub_col) * 2);
    base + idx as usize
}

fn render_bg(vram: &[u8], tilemap_base: usize, scroll_x: i32, scroll_y: i32, _width: u32, cgram: &[u8], pixels: &mut [u8]) {
    for row in 0..OW_L2_ROWS {
        for col in 0..OW_L2_COLS {
            let addr = tilemap_vram_addr(tilemap_base, col, row);
            let t0 = vram[addr] as u16;
            let t1 = vram[addr + 1] as u16;
            let x = (col * 8) as i32 - scroll_x;
            let y = (row * 8) as i32 - scroll_y;
            if x <= -8 || y <= -8 || x >= 512 || y >= 512 {
                continue;
            }
            render_tile(
                vram,
                cgram,
                (t0 | ((t1 & 3) << 8)) as usize,
                ((t1 >> 2) & 7) as usize,
                (t1 & 0x40) != 0,
                (t1 & 0x80) != 0,
                x.max(0) as u32,
                y.max(0) as u32,
                512,
                pixels,
            );
        }
    }
}

fn render_bg_full(vram: &[u8], tilemap_base: usize, width: u32, cgram: &[u8], pixels: &mut [u8]) {
    for row in 0..OW_L2_ROWS {
        for col in 0..OW_L2_COLS {
            let addr = tilemap_vram_addr(tilemap_base, col, row);
            let t0 = vram[addr] as u16;
            let t1 = vram[addr + 1] as u16;
            render_tile(
                vram,
                cgram,
                (t0 | ((t1 & 3) << 8)) as usize,
                ((t1 >> 2) & 7) as usize,
                (t1 & 0x40) != 0,
                (t1 & 0x80) != 0,
                col * 8,
                row * 8,
                width,
                pixels,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_tile(
    vram: &[u8], cgram: &[u8], tile_id: usize, palette: usize, flip_x: bool, flip_y: bool, x0: u32, y0: u32,
    width: u32, pixels: &mut [u8],
) {
    let tile_base = tile_id * 32;
    for ty in 0..8u32 {
        for tx in 0..8u32 {
            let px = if flip_x { 7 - tx } else { tx };
            let py = if flip_y { 7 - ty } else { ty };
            let row_off = tile_base + (py as usize) * 2;
            if row_off + 17 >= vram.len() {
                continue;
            }
            let b0 = vram[row_off];
            let b1 = vram[row_off + 1];
            let b2 = vram[row_off + 16];
            let b3 = vram[row_off + 17];
            let bit = 7 - px as usize;
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;
            if color_idx == 0 {
                continue;
            }
            let rgb = read_color(cgram, palette * 16 + color_idx);
            let off = (((y0 + ty) * width + x0 + tx) * 3) as usize;
            if off + 2 < pixels.len() {
                pixels[off] = rgb[0];
                pixels[off + 1] = rgb[1];
                pixels[off + 2] = rgb[2];
            }
        }
    }
}

fn read_color(cgram: &[u8], idx: usize) -> [u8; 3] {
    let off = idx * 2;
    if off + 1 >= cgram.len() {
        return [0, 0, 0];
    }
    let lo = cgram[off] as u16;
    let hi = cgram[off + 1] as u16;
    let rgb = lo | (hi << 8);
    [((rgb & 0x1F) << 3) as u8, (((rgb >> 5) & 0x1F) << 3) as u8, (((rgb >> 10) & 0x1F) << 3) as u8]
}
