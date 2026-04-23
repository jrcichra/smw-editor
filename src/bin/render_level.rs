use std::{env, path::Path, sync::Arc};

use image::{ImageBuffer, Rgb};

use smwe_emu::{emu::CheckedMem, rom::Rom as EmuRom, Cpu};

fn main() {
    let args: Vec<String> = env::args().collect();
    let level = args.iter().find_map(|a| a.strip_prefix("--level=")).and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok()).unwrap_or(0x105);
    let output = args.iter().find_map(|a| a.strip_prefix("--out=")).unwrap_or("/tmp/level.png");
    let inspect = args.iter().find_map(|a| a.strip_prefix("--inspect=")).and_then(|s| {
        let (x, y) = s.split_once(',')?;
        Some((x.parse::<u32>().ok()?, y.parse::<u32>().ok()?))
    });

    let raw = std::fs::read(Path::new("smw.smc")).expect("cannot read smw.smc");
    let rom_bytes = if raw.len() % 0x400 == 0x200 { raw[0x200..].to_vec() } else { raw };
    let mut emu_rom = EmuRom::new(rom_bytes);
    emu_rom.load_symbols(include_str!("../../symbols/SMW_U.sym"));
    let mut cpu = Cpu::new(CheckedMem::new(Arc::new(emu_rom)));

    smwe_emu::emu::decompress_sublevel(&mut cpu, level);

    let level_mode = cpu.mem.load_u8(0x1925);
    let vertical = cpu.mem.load_u8(0x5B) & 1 != 0;
    let renderer_table = cpu.mem.cart.resolve("CODE_058955").unwrap() + 9;
    let renderer = cpu.mem.load_u24(renderer_table + (level_mode as u32) * 3);
    let l2_renderers = [cpu.mem.cart.resolve("CODE_058B8D"), cpu.mem.cart.resolve("CODE_058C71")];
    let has_layer2 = l2_renderers.contains(&Some(renderer));

    let scr_len = match (vertical, has_layer2) {
        (false, false) => 0x20,
        (true, false) => 0x1C,
        (false, true) => 0x10,
        (true, true) => 0x0E,
    };
    let screens = scr_len as u32;
    let (width, height) = if vertical { (32 * 16, screens * 16 * 16) } else { (screens * 16 * 16, 27 * 16) };

    let mut pixels = vec![0u8; (width * height * 3) as usize];
    let layer = args.iter().find_map(|a| a.strip_prefix("--layer="));
    match layer {
        Some("1") => render_layer(&mut cpu, false, width, &mut pixels),
        Some("2") => render_layer(&mut cpu, true, width, &mut pixels),
        _ => {
            render_layer(&mut cpu, false, width, &mut pixels);
            render_layer(&mut cpu, true, width, &mut pixels);
        }
    }
    if let Some((x, y)) = inspect {
        inspect_block(&mut cpu, false, x, y);
        inspect_block(&mut cpu, true, x, y);
    }
    if !args.iter().any(|a| a == "--no-sprites") {
        render_sprites(&mut cpu, width, &mut pixels);
    }

    let img = ImageBuffer::<Rgb<u8>, _>::from_raw(width, height, pixels).expect("image buffer");
    img.save(output).expect("save png");
    println!("wrote {output}");
}

fn render_layer(cpu: &mut Cpu, bg: bool, width: u32, pixels: &mut [u8]) {
    let map16_bank = cpu.mem.cart.resolve("Map16Common").expect("Cannot resolve Map16Common") & 0xFF0000;
    let map16_bg = cpu.mem.cart.resolve("Map16BGTiles").expect("Cannot resolve Map16BGTiles");
    let vertical = cpu.mem.load_u8(0x5B) & if bg { 2 } else { 1 } != 0;
    let mode = cpu.mem.load_u8(0x1925);
    let renderer_table = cpu.mem.cart.resolve("CODE_058955").unwrap() + 9;
    let renderer = cpu.mem.load_u24(renderer_table + (mode as u32) * 3);
    let l2_renderers = [cpu.mem.cart.resolve("CODE_058B8D"), cpu.mem.cart.resolve("CODE_058C71")];
    let has_layer2 = l2_renderers.contains(&Some(renderer));

    let scr_len = match (vertical, has_layer2) {
        (false, false) => 0x20,
        (true, false) => 0x1C,
        (false, true) => 0x10,
        (true, true) => 0x0E,
    };
    let scr_size = if vertical { 16 * 32 } else { 16 * 27 };
    let (blocks_lo_addr, blocks_hi_addr) = match (bg, has_layer2) {
        (true, true) => {
            let offset = scr_len * scr_size;
            (0x7EC800 + offset, 0x7FC800 + offset)
        }
        (true, false) => (0x7EB900, 0x7EBD00),
        (false, _) => (0x7EC800, 0x7FC800),
    };
    let len = if has_layer2 { 256 * 27 } else { 512 * 27 };

    for idx in 0..len {
        let (block_x, block_y) = if vertical {
            let (screen, sidx) = (idx / (16 * 16), idx % (16 * 16));
            let (row, column) = (sidx / 16, sidx % 16);
            let (sub_y, sub_x) = (screen / 2, screen % 2);
            (column * 16 + sub_x * 256, row * 16 + sub_y * 256)
        } else {
            let (screen, sidx) = (idx / (16 * 27), idx % (16 * 27));
            let (row, column) = (sidx / 16, sidx % 16);
            (column * 16 + screen * 256, row * 16)
        };

        let idx_adj = if bg && !has_layer2 { idx % (16 * 27 * 2) } else { idx };
        let block_id = cpu.mem.load_u8(blocks_lo_addr + idx_adj) as u16
            | (((cpu.mem.load_u8(blocks_hi_addr + idx_adj) as u16) & 0x3F) << 8);
        let block_ptr = if bg && !has_layer2 {
            block_id as u32 * 8 + map16_bg
        } else {
            cpu.mem.load_u16(0x0FBE + block_id as u32 * 2) as u32 + map16_bank
        };

        for (sub, (off_x, off_y)) in (0..4).zip([(0u32, 0u32), (0, 8), (8, 0), (8, 8)]) {
            let t = cpu.mem.load_u16(block_ptr + sub * 2);
            render_bg_tile(&cpu.mem.vram, &cpu.mem.cgram, block_x + off_x, block_y + off_y, t, width, pixels);
        }
    }
}

fn inspect_block(cpu: &mut Cpu, bg: bool, block_x_wanted: u32, block_y_wanted: u32) {
    let map16_bank = cpu.mem.cart.resolve("Map16Common").expect("Cannot resolve Map16Common") & 0xFF0000;
    let map16_bg = cpu.mem.cart.resolve("Map16BGTiles").expect("Cannot resolve Map16BGTiles");
    let vertical = cpu.mem.load_u8(0x5B) & if bg { 2 } else { 1 } != 0;
    let mode = cpu.mem.load_u8(0x1925);
    let renderer_table = cpu.mem.cart.resolve("CODE_058955").unwrap() + 9;
    let renderer = cpu.mem.load_u24(renderer_table + (mode as u32) * 3);
    let l2_renderers = [cpu.mem.cart.resolve("CODE_058B8D"), cpu.mem.cart.resolve("CODE_058C71")];
    let has_layer2 = l2_renderers.contains(&Some(renderer));
    let scr_len = match (vertical, has_layer2) {
        (false, false) => 0x20,
        (true, false) => 0x1C,
        (false, true) => 0x10,
        (true, true) => 0x0E,
    };
    let scr_size = if vertical { 16 * 32 } else { 16 * 27 };
    let (blocks_lo_addr, blocks_hi_addr) = match (bg, has_layer2) {
        (true, true) => {
            let offset = scr_len * scr_size;
            (0x7EC800 + offset, 0x7FC800 + offset)
        }
        (true, false) => (0x7EB900, 0x7EBD00),
        (false, _) => (0x7EC800, 0x7FC800),
    };
    let len = if has_layer2 { 256 * 27 } else { 512 * 27 };
    for idx in 0..len {
        let (block_x, block_y) = if vertical {
            let (screen, sidx) = (idx / (16 * 16), idx % (16 * 16));
            let (row, column) = (sidx / 16, sidx % 16);
            let (sub_y, sub_x) = (screen / 2, screen % 2);
            (column * 16 + sub_x * 256, row * 16 + sub_y * 256)
        } else {
            let (screen, sidx) = (idx / (16 * 27), idx % (16 * 27));
            let (row, column) = (sidx / 16, sidx % 16);
            (column * 16 + screen * 256, row * 16)
        };
        if block_x != block_x_wanted || block_y != block_y_wanted {
            continue;
        }
        let idx_adj = if bg && !has_layer2 { idx % (16 * 27 * 2) } else { idx };
        let lo = cpu.mem.load_u8(blocks_lo_addr + idx_adj) as u16;
        let hi_raw = cpu.mem.load_u8(blocks_hi_addr + idx_adj) as u16;
        let block_id = lo | ((hi_raw & 0x3F) << 8);
        let block_ptr = if bg && !has_layer2 {
            block_id as u32 * 8 + map16_bg
        } else {
            cpu.mem.load_u16(0x0FBE + block_id as u32 * 2) as u32 + map16_bank
        };
        println!(
            "{} ({:03},{:03}) idx={} idx_adj={} lo={:02X} hi={:02X} block={:03X} ptr={:06X}",
            if bg { "L2" } else { "L1" },
            block_x,
            block_y,
            idx,
            idx_adj,
            lo,
            hi_raw,
            block_id,
            block_ptr
        );
        for sub in 0..4u32 {
            let t = cpu.mem.load_u16(block_ptr + sub * 2);
            println!("  sub{}={:04X}", sub, t);
        }
        break;
    }
}

fn render_sprites(cpu: &mut Cpu, width: u32, pixels: &mut [u8]) {
    smwe_emu::emu::exec_sprites(cpu);
    for spr in (0..64).rev() {
        let x = cpu.mem.load_u8(0x300 + spr * 4) as u32;
        let y = cpu.mem.load_u8(0x301 + spr * 4) as u32;
        if y >= 0xE0 {
            continue;
        }
        let tile = cpu.mem.load_u16(0x302 + spr * 4);
        let size = cpu.mem.load_u8(0x460 + spr);
        if size & 0x02 != 0 {
            let (xn, xf) = if tile & 0x4000 == 0 { (0, 8) } else { (8, 0) };
            let (yn, yf) = if tile & 0x8000 == 0 { (0, 8) } else { (8, 0) };
            render_sp_tile(&cpu.mem.vram, &cpu.mem.cgram, x + xn, y + yn, tile, width, pixels);
            render_sp_tile(&cpu.mem.vram, &cpu.mem.cgram, x + xf, y + yn, tile + 1, width, pixels);
            render_sp_tile(&cpu.mem.vram, &cpu.mem.cgram, x + xn, y + yf, tile + 16, width, pixels);
            render_sp_tile(&cpu.mem.vram, &cpu.mem.cgram, x + xf, y + yf, tile + 17, width, pixels);
        } else {
            render_sp_tile(&cpu.mem.vram, &cpu.mem.cgram, x, y, tile, width, pixels);
        }
    }
}

fn render_bg_tile(vram: &[u8], cgram: &[u8], x: u32, y: u32, t: u16, width: u32, pixels: &mut [u8]) {
    let tile = (t & 0x3FF) as usize;
    let pal = ((t >> 10) & 0x7) as usize;
    render_tile(vram, cgram, tile, pal, (t & 0x4000) != 0, (t & 0x8000) != 0, x, y, width, pixels);
}

fn render_sp_tile(vram: &[u8], cgram: &[u8], x: u32, y: u32, t: u16, width: u32, pixels: &mut [u8]) {
    let tile = ((t & 0x1FF) + 0x600) as usize;
    let pal = (((t >> 9) & 0x7) + 8) as usize;
    render_tile(vram, cgram, tile, pal, (t & 0x4000) != 0, (t & 0x8000) != 0, x, y, width, pixels);
}

#[allow(clippy::too_many_arguments)]
fn render_tile(
    vram: &[u8], cgram: &[u8], tile_id: usize, palette: usize, flip_x: bool, flip_y: bool, x0: u32, y0: u32, width: u32,
    pixels: &mut [u8],
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
