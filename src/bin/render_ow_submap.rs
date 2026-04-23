use std::{env, path::Path, sync::Arc};

use image::{ImageBuffer, Rgb};

use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

const VRAM_L1_TILEMAP_BASE: usize = 0x2000 * 2;
const VRAM_L2_TILEMAP_BASE: usize = 0x3000 * 2;
const OW_L2_COLS: u32 = 64;
const OW_L2_ROWS: u32 = 64;

fn main() {
    let args: Vec<String> = env::args().collect();
    let submap = args.iter().find_map(|a| a.strip_prefix("--submap=")).and_then(|s| s.parse::<u8>().ok()).unwrap_or(3);
    let output = args
        .iter()
        .find_map(|a| a.strip_prefix("--out="))
        .unwrap_or("ow_render.png");
    let full = args.iter().any(|a| a == "--full");

    let rom_path = Path::new("smw.smc");
    let raw = std::fs::read(rom_path).expect("cannot read smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

    activate_all_overworld_events(&mut cpu);
    smwe_emu::emu::load_overworld(&mut cpu, submap);

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
