//! Debug binary to render overworld L1/L2 tiles to PPM for comparison.
//!
//! Usage: cargo run --bin debug_ow [-- --submap N] [--wireframe] [--l1-y-offset=8]

use std::{env, sync::Arc};

fn main() {
    let args: Vec<String> = env::args().collect();
    let submap = args.iter().find_map(|a| a.strip_prefix("--submap=")).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);
    let wireframe = args.iter().any(|a| a == "--wireframe");
    let l1_y_offset =
        args.iter().find_map(|a| a.strip_prefix("--l1-y-offset=")).and_then(|s| s.parse::<u8>().ok()).unwrap_or(0);

    let rom_path = std::path::Path::new("smw.smc");
    if !rom_path.exists() {
        eprintln!("Error: smw.smc not found");
        std::process::exit(1);
    }

    log4rs::init_file("log4rs.yaml", Default::default()).ok();

    run_debug(rom_path, submap, wireframe, l1_y_offset);
}

fn run_debug(rom_path: &std::path::Path, submap: u8, wireframe: bool, l1_y_offset: u8) {
    let raw = std::fs::read(rom_path).expect("Cannot read ROM");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let rom_bytes_for_gfx = rom_bytes.clone(); // Keep a copy for GFX loading
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

        // OWL1CharData order: word 0->TL, word 1->TR, word 2->BL, word 3->BR
        for i in 0..4 {
            let sub_tile = cpu.mem.load_u16(gfx_addr + i as u32 * 2);
            let tile_num = sub_tile & 0x3FF;
            let pal = (sub_tile >> 10) & 7;
            let names = ["TL", "TR", "BL", "BR"];
            println!("    {}: ${:04X} (tile=${:03X} pal={})", names[i as usize], sub_tile, tile_num, pal);
        }
    }

    // Force all events to be completed so level icons appear
    for i in 0x1F02u32..=0x1F60 {
        cpu.mem.store_u8(i, 0xFF);
    }

    // Load overworld GFX files (1D and 1E) into VRAM
    // These contain the L1 icons, paths, and level graphics
    let gfx_result = load_overworld_gfx(&rom_bytes_for_gfx, &mut cpu);
    if let Err(e) = gfx_result {
        eprintln!("Warning: Could not load overworld GFX: {}", e);
    }
    println!("Missing tiles in range:");
    for tile_id in 0x100..=0x200 {
        let tile_base = tile_id * 32;
        let has_data = cpu.mem.vram[tile_base..tile_base + 4].iter().any(|&b| b != 0);
        if !has_data {
            print!("${:03X} ", tile_id);
        }
    }
    println!();

    let vram = &cpu.mem.vram;
    let cgram = &cpu.mem.cgram;
    let wram = &cpu.mem.wram;

    // Debug: Check VRAM contents for L1 tiles including $1D8-$1E0 range
    println!("\n=== VRAM Debug (tiles $1D0-$1E0) ===");
    for tile_id in 0x1D0..=0x1E0 {
        let tile_base = tile_id * 32;
        // Check if tile has non-zero data
        let has_data = vram[tile_base..tile_base + 32].iter().any(|&b| b != 0);
        let first_bytes = &vram[tile_base..tile_base + 8];
        if has_data {
            println!("Tile ${:03X}: {:02X?}", tile_id, first_bytes);
        }
    }

    // L2 is 64×64 tiles total, arranged as 4 quadrants of 32×32
    let l2_cols = 64u32;
    let l2_rows = 64u32;

    // Render L2 first (background)
    let l2_pixels = render_l2(&wram[(0x7F4000 - 0x7E0000) as usize..], l2_cols, l2_rows, vram, cgram);

    // Debug: Check first few L1 tiles actually rendered
    println!("\n=== L1 Tiles Being Rendered ===");
    let l1_data_debug = &wram[(0x7EC800 - 0x7E0000) as usize..];
    let m16ptr_data_debug = &wram[(0x7E0FBE - 0x7E0000) as usize..];

    println!("L1 data first 16 bytes: {:02X?}", &l1_data_debug[..16]);
    println!("L1 data bytes 256-271: {:02X?}", &l1_data_debug[256..272]);
    println!("L1 data bytes 512-527: {:02X?}", &l1_data_debug[512..528]);

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
        wireframe,
        l1_y_offset,
    );

    // Write individual layers as PNG
    write_png("debug_l2.png", l2_cols * 8, l2_rows * 8, &l2_pixels);
    write_png("debug_composite.png", l2_cols * 8, l2_rows * 8, &final_pixels);

    println!("\nWrote:");
    println!("  - debug_l2.png (L2 background only)");
    println!("  - debug_composite.png (L1+L2 combined)");
    if wireframe {
        println!("  (with yellow wireframe borders around Map16 blocks)");
    }
    println!("\nUsage: cargo run --bin debug_ow [-- --submap=N] [--wireframe] [--l1-y-offset=N]");
    println!("  --l1-y-offset=8 : Apply 8px Y offset to Layer 1 (for testing alignment)");
    println!("Reference: SMW_Final_Overworld.png (from TCRF)");
}

fn render_l2(wram: &[u8], cols: u32, rows: u32, vram: &[u8], cgram: &[u8]) -> Vec<u8> {
    let width_px = cols * 8;
    let height_px = rows * 8;
    let mut pixels = vec![0u8; (width_px * height_px * 3) as usize];

    // L2 tilemap is stored as 4 quadrants of 32x32 tiles
    // Quadrant layout: TL=0, TR=1, BL=2, BR=3
    // Each quadrant is 32*32*2 = 2048 bytes
    for row in 0..rows {
        for col in 0..cols {
            // Determine which quadrant we're in
            let quadrant = ((row / 32) * 2) + (col / 32); // 0=TL, 1=TR, 2=BL, 3=BR
            let sub_row = row % 32;
            let sub_col = col % 32;

            // Index within quadrant: (row * 32 + col) * 2
            let quadrant_offset = (quadrant * 32 * 32 * 2) as usize;
            let idx = quadrant_offset + (((sub_row * 32 + sub_col) * 2) as usize);

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
    l2_rows: u32, _ptr_base: u32, char_bank: u32, wireframe: bool, l1_y_offset: u8,
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

            // Debug: Print tile 0x99 info
            if tile_id == 0x99 {
                eprintln!("Tile 0x99: ptr=${:04X}, snes=${:06X}, rom_ofs=${:06X}", char_ptr, snes_addr, rom_offset);
            }

            // Render 4 sub-tiles from OWL1CharData: word 0->TL, word 1->TR, word 2->BL, word 3->BR
            // Layout in ROM: [TL][TR][BL][BR] = 8 bytes total
            let sub_tiles = [
                (rom_bytes.get(rom_offset + 0).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 1).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 2).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 3).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 4).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 5).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 6).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 7).copied().unwrap_or(0) as u16) << 8),
            ];

            if tile_id == 0x99 {
                eprintln!(
                    "  Sub-tiles: TL=${:04X} TR=${:04X} BL=${:04X} BR=${:04X}",
                    sub_tiles[0], sub_tiles[1], sub_tiles[2], sub_tiles[3]
                );
            }

            // Position in output
            let px = col * 16;
            let py = row * 16 + l1_y_offset as u32;

            if px + 16 > l2_cols * 8 || py + 16 > l2_rows * 8 {
                continue;
            }

            // Render 4 sub-tiles from OWL1CharData: word 0->TL, word 1->TR, word 2->BL, word 3->BR
            // Layout in ROM: [TL][TR][BL][BR] = 8 bytes total
            let sub_tiles = [
                (rom_bytes.get(rom_offset + 0).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 1).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 2).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 3).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 4).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 5).copied().unwrap_or(0) as u16) << 8),
                (rom_bytes.get(rom_offset + 6).copied().unwrap_or(0) as u16)
                    | ((rom_bytes.get(rom_offset + 7).copied().unwrap_or(0) as u16) << 8),
            ];

            let offsets = [(0u32, 0u32), (8u32, 0u32), (0u32, 8u32), (8u32, 8u32)];
            for (i, (ox, oy)) in offsets.iter().enumerate() {
                let sub_tile = sub_tiles[i];
                let tile_num = (sub_tile & 0x3FF) as usize;
                let palette = ((sub_tile >> 10) & 7) as usize;
                let flip_x = (sub_tile & 0x4000) != 0;
                let flip_y = (sub_tile & 0x8000) != 0;

                // Only render non-transparent tiles
                render_tile(vram, cgram, tile_num, palette, flip_x, flip_y, px + ox, py + oy, width_px, pixels);
            }

            // Draw wireframe border around Map16 block if enabled
            if wireframe && tile_id != 0 {
                draw_map16_border(pixels, px, py, width_px, [255, 255, 0]); // Yellow border
            }
        }
    }
}

fn convert_3bpp_to_4bpp(data_3bpp: &[u8]) -> Vec<u8> {
    // 3BPP: 24 bytes per tile (bitplanes 0-1 in bytes 0-15, bitplane 2 in bytes 16-23)
    // 4BPP: 32 bytes per tile (bitplanes 0-1 in bytes 0-15, bitplanes 2-3 in bytes 16-31)
    // bitplane 3 is all zeros for 3BPP source
    let num_tiles = data_3bpp.len() / 24;
    let mut data_4bpp = Vec::with_capacity(num_tiles * 32);

    for tile_idx in 0..num_tiles {
        let tile_offset_3bpp = tile_idx * 24;
        let tile_offset_4bpp = tile_idx * 32;

        // Copy bitplanes 0-1 (bytes 0-15)
        for i in 0..16 {
            data_4bpp.push(data_3bpp[tile_offset_3bpp + i]);
        }

        // Expand bitplane 2 into bitplanes 2-3 (bytes 16-31)
        // In 4BPP, bytes are interleaved: [bp2_row0, bp3_row0, bp2_row1, bp3_row1, ...]
        for row in 0..8 {
            data_4bpp.push(data_3bpp[tile_offset_3bpp + 16 + row]); // bitplane 2
            data_4bpp.push(0); // bitplane 3 (zeros)
        }
    }

    data_4bpp
}

fn load_overworld_gfx(rom_bytes: &[u8], cpu: &mut smwe_emu::Cpu) -> Result<(), Box<dyn std::error::Error>> {
    // GFX1D and GFX1E are 3BPP files containing overworld L1 graphics
    // They need to be decompressed, converted to 4BPP, and loaded into VRAM

    use smwe_rom::compression::lc_lz2;

    // GFX1D: Overworld graphics file at PC address (LoROM $0ADC88)
    // PC = ((SNES & 0x7F0000) >> 1) | (SNES & 0x7FFF)
    let gfx1d_pc = ((0x0ADC88 & 0x7F0000) >> 1) | (0x0ADC88 & 0x7FFF);
    let gfx1d_size = 2551;

    // GFX1E: Overworld graphics file at PC address (LoROM $0AE67F)
    let gfx1e_pc = ((0x0AE67F & 0x7F0000) >> 1) | (0x0AE67F & 0x7FFF);
    let gfx1e_size = 1988;

    // Decompress GFX1D, convert to 4BPP, and load into VRAM at $2400 (tile $120)
    // This provides tiles $120-$19F to fill gaps like $122
    // GFX1D: Overworld aesthetic (trees, rocks, Star Road moon)
    match lc_lz2::decompress(&rom_bytes[gfx1d_pc..gfx1d_pc + gfx1d_size], false) {
        Ok(decompressed) => {
            let data_4bpp = convert_3bpp_to_4bpp(&decompressed);
            let vram_base = 0x2400; // VRAM address $2400 = tile $120
            for (i, byte) in data_4bpp.iter().enumerate() {
                if vram_base + i < cpu.mem.vram.len() {
                    cpu.mem.vram[vram_base + i] = *byte;
                }
            }
            eprintln!(
                "Loaded GFX1D: {} tiles into VRAM at ${:04X} (tile ${:03X})",
                data_4bpp.len() / 32,
                vram_base,
                vram_base / 32
            );
        }
        Err(e) => eprintln!("Failed to decompress GFX1D: {:?}", e),
    }

    // Decompress GFX1E, convert to 4BPP, and load into VRAM at $4C00 (tile $260)
    // This provides tiles $260-$2DF to cover $279 etc.
    // GFX1E: Overworld level tiles (castles, Yoshi's house, signs)
    match lc_lz2::decompress(&rom_bytes[gfx1e_pc..gfx1e_pc + gfx1e_size], false) {
        Ok(decompressed) => {
            let data_4bpp = convert_3bpp_to_4bpp(&decompressed);
            let vram_base = 0x4C00; // VRAM address $4C00 = tile $260
            for (i, byte) in data_4bpp.iter().enumerate() {
                if vram_base + i < cpu.mem.vram.len() {
                    cpu.mem.vram[vram_base + i] = *byte;
                }
            }
            eprintln!(
                "Loaded GFX1E: {} tiles into VRAM at ${:04X} (tile ${:03X})",
                data_4bpp.len() / 32,
                vram_base,
                vram_base / 32
            );
        }
        Err(e) => eprintln!("Failed to decompress GFX1E: {:?}", e),
    }

    Ok(())
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
    // VRAM stores tiles as 4BPP = 32 bytes per tile (even if source is 3BPP)
    let tile_base = tile_id * 32;

    for ty in 0..8u32 {
        for tx in 0..8u32 {
            let px = if flip_x { 7 - tx } else { tx };
            let py = if flip_y { 7 - ty } else { ty };

            // 4BPP format: bitplanes stored in VRAM
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

fn draw_map16_border(pixels: &mut [u8], x: u32, y: u32, width: u32, color: [u8; 3]) {
    // Draw 1-pixel border around 16x16 Map16 block
    for i in 0..16 {
        // Top edge
        let off = ((y * width + x + i) * 3) as usize;
        if off + 2 < pixels.len() {
            pixels[off] = color[0];
            pixels[off + 1] = color[1];
            pixels[off + 2] = color[2];
        }
        // Bottom edge
        let off = (((y + 15) * width + x + i) * 3) as usize;
        if off + 2 < pixels.len() {
            pixels[off] = color[0];
            pixels[off + 1] = color[1];
            pixels[off + 2] = color[2];
        }
        // Left edge
        let off = (((y + i) * width + x) * 3) as usize;
        if off + 2 < pixels.len() {
            pixels[off] = color[0];
            pixels[off + 1] = color[1];
            pixels[off + 2] = color[2];
        }
        // Right edge
        let off = (((y + i) * width + x + 15) * 3) as usize;
        if off + 2 < pixels.len() {
            pixels[off] = color[0];
            pixels[off + 1] = color[1];
            pixels[off + 2] = color[2];
        }
    }
}

fn write_png(filename: &str, width: u32, height: u32, pixels: &[u8]) {
    use image::{ImageBuffer, Rgb};
    let img =
        ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, pixels.to_vec()).expect("Failed to create image buffer");
    img.save(filename).expect("Failed to save PNG");
}
