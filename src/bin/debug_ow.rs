//! Debug binary to render overworld L1/L2 tiles to PPM for comparison.
//!
//! Usage: cargo run --bin debug_ow [-- --submap N]

use std::{env, sync::Arc};

fn main() {
    let args: Vec<String> = env::args().collect();
    let submap = args.iter().find_map(|a| a.strip_prefix("--submap=")).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);

    let rom_path = std::path::Path::new("smw.smc");
    if !rom_path.exists() {
        eprintln!("Error: smw.smc not found");
        std::process::exit(1);
    }

    log4rs::init_file("log4rs.yaml", Default::default()).ok();

    run_debug(rom_path, submap);
}

fn run_debug(rom_path: &std::path::Path, submap: u8) {
    let raw = std::fs::read(rom_path).expect("Cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = smwe_emu::rom::Rom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = smwe_emu::Cpu::new(smwe_emu::emu::CheckedMem::new(Arc::new(emu_rom)));

    println!("Loading overworld submap {}...", submap);
    smwe_emu::emu::load_overworld(&mut cpu, submap);

    // Debug: Check Map16Pointers and read actual sub-tile data (before borrowing mem fields)
    println!("\n=== Map16Pointers and Sub-tile Data ===");
    let ptr_base: u32 = 0x7E0FBE;
    let char_bank: u32 = 0x05_0000;

    for tile_id in [0x01u8, 0x02, 0x03] {
        let char_ptr = cpu.mem.load_u16(ptr_base + tile_id as u32 * 2) as u32;
        let gfx_addr = char_bank | char_ptr;

        println!("\nTile {:02X}:", tile_id);
        println!("  Map16Pointer: ${:04X}", char_ptr);
        println!("  GFX Address:  ${:06X}", gfx_addr);
        println!("  Sub-tiles:");

        for i in 0..4 {
            let sub_tile = cpu.mem.load_u16(gfx_addr + i as u32 * 2);
            let tile_num = sub_tile & 0x3FF;
            let pal = (sub_tile >> 10) & 7;
            let names = ["TL", "BL", "TR", "BR"];
            println!("    {}: ${:04X} (tile=${:03X} pal={})", names[i as usize], sub_tile, tile_num, pal);
        }
    }

    let vram = &cpu.mem.vram;
    let cgram = &cpu.mem.cgram;
    let wram = &cpu.mem.wram;

    // L2 is always 64×64 tiles (512×512 pixels) to match L1's 512×512 pixel area
    let l2_cols = 64u32;
    let l2_rows = 64u32;

    // Render L2 first (background)
    let l2_pixels = render_l2(&wram[(0x7F4000 - 0x7E0000) as usize..], l2_cols, l2_rows, vram, cgram);

    // Debug: Check first few L1 tiles actually rendered
    println!("\n=== L1 Tiles Being Rendered ===");
    let l1_data_debug = &wram[(0x7EC800 - 0x7E0000) as usize..];
    let m16ptr_data_debug = &wram[(0x7E0FBE - 0x7E0000) as usize..];

    println!("L1 data first 16 bytes: {:02X?}", &l1_data_debug[..16]);

    for row in 0..10 {
        for col in 0..10 {
            let idx = ow_l1_addr(col as u32, row as u32);
            let tile_id = l1_data_debug[idx] as u32;
            if tile_id != 0 {
                let ptr_idx = tile_id as usize * 2;
                let ptr_lo = m16ptr_data_debug[ptr_idx];
                let ptr_hi = m16ptr_data_debug[ptr_idx + 1];
                let char_ptr = (ptr_lo as u32) | ((ptr_hi as u32) << 8);
                println!("Row {} Col {}: tile_id={:02X} ptr=${:04X}", row, col, tile_id, char_ptr);
            }
        }
    }

    // Render L1 on top (proper Map16 tiles)
    let mut final_pixels = l2_pixels.clone();
    let l1_data = &wram[(0x7EC800 - 0x7E0000) as usize..];
    let m16ptr_data = &wram[(0x7E0FBE - 0x7E0000) as usize..];
    render_l1_proper(
        l1_data,
        m16ptr_data,
        submap,
        vram,
        cgram,
        &mut final_pixels,
        l2_cols,
        l2_rows,
        ptr_base,
        char_bank,
    );

    // Write individual layers
    write_ppm("debug_l2.ppm", l2_cols * 8, l2_rows * 8, &l2_pixels);
    write_ppm("debug_composite.ppm", l2_cols * 8, l2_rows * 8, &final_pixels);

    println!("\nWrote:");
    println!("  - debug_l2.ppm (L2 background only)");
    println!("  - debug_composite.ppm (L1+L2 combined)");
    println!("Reference: SMW_Final_Overworld.png (from TCRF)");
}

fn render_l2(wram: &[u8], cols: u32, rows: u32, vram: &[u8], cgram: &[u8]) -> Vec<u8> {
    let width_px = cols * 8;
    let height_px = rows * 8;
    let mut pixels = vec![0u8; (width_px * height_px * 3) as usize];

    for row in 0..rows {
        for col in 0..cols {
            let idx = ((row * cols + col) * 2) as usize;
            if idx + 1 >= wram.len() {
                continue;
            }
            let t0 = wram[idx];
            let t1 = wram[idx + 1];
            let tile_num = t0 as u16;
            let attr = t1 as u16;

            let tile_id = (tile_num | ((attr & 3) << 8)) as usize;
            let palette = ((attr >> 2) & 7) as usize;
            let flip_x = (attr & 0x40) != 0;
            let flip_y = (attr & 0x80) != 0;

            render_tile(vram, cgram, tile_id, palette, flip_x, flip_y, col * 8, row * 8, width_px, &mut pixels);
        }
    }
    pixels
}

fn render_l1_proper(
    l1_data: &[u8], m16ptr_data: &[u8], submap: u8, vram: &[u8], cgram: &[u8], pixels: &mut [u8], l2_cols: u32,
    l2_rows: u32, _ptr_base: u32, char_bank: u32,
) {
    // L1 is 32x32 Map16 blocks, each 16x16 pixels
    // But we're rendering at 8x8 sub-tile resolution
    let l1_offset = if submap != 0 { 0x400usize } else { 0usize };
    let width_px = l2_cols * 8;

    // Read ROM to get OWL1CharData
    let raw = std::fs::read("smw.smc").expect("Cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { &raw[0x200..] } else { &raw };

    for row in 0..32u32 {
        for col in 0..32u32 {
            let idx = ow_l1_addr(col, row);
            if l1_offset + idx >= l1_data.len() {
                continue;
            }
            let tile_id = l1_data[l1_offset + idx] as u32;

            // Skip empty tiles
            if tile_id == 0 {
                continue;
            }

            // Look up Map16 pointer from m16ptr_data (Map16Pointers at $7E0FBE)
            let ptr_idx = tile_id as usize * 2;
            if ptr_idx + 1 >= m16ptr_data.len() {
                continue;
            }
            let ptr_lo = m16ptr_data[ptr_idx];
            let ptr_hi = m16ptr_data[ptr_idx + 1];
            let char_ptr = (ptr_lo as u32) | ((ptr_hi as u32) << 8);

            // Calculate ROM address using LoRom mapping
            let snes_addr = char_bank | char_ptr;
            let rom_offset = (((snes_addr & 0x7F0000) >> 1) | (snes_addr & 0x7FFF)) as usize;

            // Position in output
            let px = col * 16;
            let py = row * 16;

            if px + 16 > l2_cols * 8 || py + 16 > l2_rows * 8 {
                continue;
            }

            // Render 4 sub-tiles
            let offsets = [(0u32, 0u32), (0u32, 8u32), (8u32, 0u32), (8u32, 8u32)];
            for (si, (ox, oy)) in offsets.iter().enumerate() {
                if rom_offset + si * 2 + 1 < rom_bytes.len() {
                    let st_lo = rom_bytes[rom_offset + si * 2];
                    let st_hi = rom_bytes[rom_offset + si * 2 + 1];
                    let sub_tile = (st_lo as u16) | ((st_hi as u16) << 8);

                    let tile_num = (sub_tile & 0x3FF) as usize;
                    let palette = ((sub_tile >> 10) & 7) as usize;
                    let flip_x = (sub_tile & 0x4000) != 0;
                    let flip_y = (sub_tile & 0x8000) != 0;

                    // Only render non-transparent tiles
                    render_tile(vram, cgram, tile_num, palette, flip_x, flip_y, px + ox, py + oy, width_px, pixels);
                }
            }
        }
    }
}

fn ow_l1_addr(col: u32, row: u32) -> usize {
    let x_part = (col & 0x0F) | ((col & 0x10) << 4);
    let y_part = ((row & 0x0F) << 4) | ((row & 0x10) << 5);
    (x_part + y_part) as usize
}

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

            let bit = 7 - (px & 7) as usize;
            let color_idx =
                (((b0 >> bit) & 1) | (((b1 >> bit) & 1) << 1) | (((b2 >> bit) & 1) << 2) | (((b3 >> bit) & 1) << 3))
                    as usize;

            // Don't render transparent color 0
            if color_idx == 0 {
                continue;
            }

            let pal_off = palette * 16;
            let final_color_idx = pal_off + color_idx;
            let color = read_color(cgram, final_color_idx);

            let off = (((y0 + ty) * width + x0 + tx) * 3) as usize;
            if off + 2 < pixels.len() {
                pixels[off] = color[0];
                pixels[off + 1] = color[1];
                pixels[off + 2] = color[2];
            }
        }
    }
}

fn read_color(cgram: &[u8], idx: usize) -> [u8; 3] {
    let off = idx * 2;
    if off + 1 >= cgram.len() {
        return [0, 0, 0];
    }
    let lo = cgram[off];
    let hi = cgram[off + 1];
    let rgb = ((hi as u16) << 8) | (lo as u16);
    let r = ((rgb >> 0) & 0x1F) << 3;
    let g = ((rgb >> 5) & 0x1F) << 3;
    let b = ((rgb >> 10) & 0x1F) << 3;
    [r as u8, g as u8, b as u8]
}

fn write_ppm(filename: &str, width: u32, height: u32, pixels: &[u8]) {
    use std::io::Write;
    let mut f = std::fs::File::create(filename).expect("Cannot create PPM");
    write!(f, "P6\n{} {}\n255\n", width, height).expect("Write failed");
    f.write_all(pixels).expect("Write failed");
}
