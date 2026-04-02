use egui::TextureHandle;
use smwe_emu::{emu::SpriteOamTile, Cpu};

/// Number of Map16 blocks in the tileset (9-bit IDs: 0-511).
const NUM_BLOCKS: usize = 512;

/// Grid layout for the tile picker texture.
const COLS: usize = 16;
const ROWS: usize = NUM_BLOCKS / COLS; // 32

/// Size of each Map16 block in pixels (2×2 8×8 sub-tiles).
const BLOCK_PX: usize = 16;

/// Width/height of the full tile picker texture.
const TEX_W: usize = COLS * BLOCK_PX; // 256
const TEX_H: usize = ROWS * BLOCK_PX; // 512
const BG_NUM_BLOCKS: usize = 256;
const BG_ROWS: usize = BG_NUM_BLOCKS / COLS; // 16
const BG_TEX_H: usize = BG_ROWS * BLOCK_PX; // 256

pub(super) struct TilePicker {
    pixels: Vec<u8>, // RGBA, TEX_W * TEX_H * 4
    texture: Option<TextureHandle>,
    dirty: bool,
}

pub(super) struct BgTilePicker {
    pixels: Vec<u8>,
    texture: Option<TextureHandle>,
    dirty: bool,
}

impl TilePicker {
    pub fn new() -> Self {
        Self { pixels: vec![0u8; TEX_W * TEX_H * 4], texture: None, dirty: true }
    }

    /// Decode all 512 Map16 blocks from the current VRAM/CGRAM state.
    pub fn rebuild(&mut self, cpu: &mut Cpu) {
        self.pixels.fill(0);

        let map16_bank = cpu.mem.cart.resolve("Map16Common").unwrap_or(0) & 0xFF0000;

        // Pre-compute all block pointers (needs mutable load_u16).
        let mut block_ptrs = [0u32; NUM_BLOCKS];
        for block_id in 0..NUM_BLOCKS {
            let ptr_lo = 0x0FBE + block_id * 2;
            if ptr_lo + 1 < 0x10000 {
                block_ptrs[block_id] = cpu.mem.load_u16(ptr_lo as u32) as u32 + map16_bank;
            }
        }

        let vram = &cpu.mem.vram;
        let cgram = &cpu.mem.cgram;

        for block_id in 0..NUM_BLOCKS {
            let col = block_id % COLS;
            let row = block_id / COLS;
            let x0 = (col * BLOCK_PX) as u32;
            let y0 = (row * BLOCK_PX) as u32;

            let block_ptr = block_ptrs[block_id];
            if block_ptr == map16_bank {
                continue;
            }

            let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
            for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
                let tile_word_addr = block_ptr + (sub_i as u32) * 2;
                let lo = cpu.mem.cart.read(tile_word_addr).unwrap_or(0);
                let hi = cpu.mem.cart.read(tile_word_addr + 1).unwrap_or(0);
                let t = lo as u16 | ((hi as u16) << 8);

                render_sub_tile(vram, cgram, t, x0 + sx, y0 + sy, &mut self.pixels, TEX_W);
            }
        }

        self.dirty = true;
    }

    /// Get the egui texture handle, creating or updating as needed.
    pub fn texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, TEX_H], &self.pixels);
            let handle = ctx.load_texture("level_tile_picker", image, egui::TextureOptions::NEAREST);
            self.texture = Some(handle);
            self.dirty = false;
        }
        if self.dirty {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, TEX_H], &self.pixels);
            self.texture.as_mut().unwrap().set(image, egui::TextureOptions::NEAREST);
            self.dirty = false;
        }
        self.texture.as_ref().unwrap().clone()
    }

    /// Convert a pixel position within the texture to a block ID (0-511).
    pub fn block_at_pixel(&self, px: f32, py: f32) -> Option<u16> {
        if px < 0.0 || py < 0.0 {
            return None;
        }
        let col = (px as usize) / BLOCK_PX;
        let row = (py as usize) / BLOCK_PX;
        if col < COLS && row < ROWS {
            let id = (row * COLS + col) as u16;
            if id < NUM_BLOCKS as u16 {
                Some(id)
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl BgTilePicker {
    pub fn new() -> Self {
        Self { pixels: vec![0u8; TEX_W * BG_TEX_H * 4], texture: None, dirty: true }
    }

    pub fn rebuild(&mut self, cpu: &mut Cpu) {
        self.pixels.fill(0);
        let map16_bg = cpu.mem.cart.resolve("Map16BGTiles").unwrap_or(0);

        for block_id in 0..BG_NUM_BLOCKS {
            let col = block_id % COLS;
            let row = block_id / COLS;
            let x0 = (col * BLOCK_PX) as u32;
            let y0 = (row * BLOCK_PX) as u32;
            let block_ptr = map16_bg + block_id as u32 * 8;
            let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
            for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
                let tile_word_addr = block_ptr + (sub_i as u32) * 2;
                let lo = cpu.mem.cart.read(tile_word_addr).unwrap_or(0);
                let hi = cpu.mem.cart.read(tile_word_addr + 1).unwrap_or(0);
                let t = lo as u16 | ((hi as u16) << 8);
                render_sub_tile(&cpu.mem.vram, &cpu.mem.cgram, t, x0 + sx, y0 + sy, &mut self.pixels, TEX_W);
            }
        }

        self.dirty = true;
    }

    pub fn texture(&mut self, ctx: &egui::Context) -> TextureHandle {
        if self.texture.is_none() {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, BG_TEX_H], &self.pixels);
            let handle = ctx.load_texture("level_bg_tile_picker", image, egui::TextureOptions::NEAREST);
            self.texture = Some(handle);
            self.dirty = false;
        }
        if self.dirty {
            let image = egui::ColorImage::from_rgba_unmultiplied([TEX_W, BG_TEX_H], &self.pixels);
            self.texture.as_mut().unwrap().set(image, egui::TextureOptions::NEAREST);
            self.dirty = false;
        }
        self.texture.as_ref().unwrap().clone()
    }

    pub fn block_at_pixel(&self, px: f32, py: f32) -> Option<u16> {
        if px < 0.0 || py < 0.0 {
            return None;
        }
        let col = (px as usize) / BLOCK_PX;
        let row = (py as usize) / BLOCK_PX;
        if col < COLS && row < BG_ROWS {
            Some((row * COLS + col) as u16)
        } else {
            None
        }
    }

    pub fn block_grid_pos(&self, block_id: u8) -> Option<(usize, usize)> {
        let idx = block_id as usize;
        (idx < BG_NUM_BLOCKS).then_some((idx % COLS, idx / COLS))
    }
}

/// Decode a single 8×8 SNES 4bpp tile from VRAM and write RGBA pixels.
fn render_sub_tile(vram: &[u8], cgram: &[u8], t: u16, x0: u32, y0: u32, pixels: &mut [u8], stride: usize) {
    let tile_num = (t & 0x3FF) as usize;
    let pal = ((t >> 10) & 0x7) as usize;
    let flip_x = (t & 0x4000) != 0;
    let flip_y = (t & 0x8000) != 0;

    let tile_base = tile_num * 32;
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

            let pal_idx = pal * 16 + color_idx;
            let off_color = pal_idx * 2;
            if off_color + 1 >= cgram.len() {
                continue;
            }
            let lo = cgram[off_color] as u16;
            let hi = cgram[off_color + 1] as u16;
            let rgb = lo | (hi << 8);

            let r = ((rgb & 0x1F) << 3) as u8;
            let g = (((rgb >> 5) & 0x1F) << 3) as u8;
            let b = (((rgb >> 10) & 0x1F) << 3) as u8;

            let px_abs = x0 + tx;
            let py_abs = y0 + ty;
            let off = ((py_abs as usize) * stride + px_abs as usize) * 4;
            if off + 3 < pixels.len() {
                pixels[off] = r;
                pixels[off + 1] = g;
                pixels[off + 2] = b;
                pixels[off + 3] = 255;
            }
        }
    }
}

/// Render a single Map16 block (16×16) into a 16×16 RGBA pixel buffer.
/// Returns the pixels and a 16×16 egui::ColorImage.
pub(super) fn render_block_image(block_id: u16, cpu: &mut Cpu) -> egui::ColorImage {
    let mut pixels = vec![0u8; 16 * 16 * 4];
    let map16_bank = cpu.mem.cart.resolve("Map16Common").unwrap_or(0) & 0xFF0000;
    let ptr_lo = 0x0FBE + (block_id as usize) * 2;
    if ptr_lo + 1 < 0x10000 {
        let block_ptr = cpu.mem.load_u16(ptr_lo as u32) as u32 + map16_bank;
        let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
        for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
            let tile_word_addr = block_ptr + (sub_i as u32) * 2;
            let lo = cpu.mem.cart.read(tile_word_addr).unwrap_or(0);
            let hi = cpu.mem.cart.read(tile_word_addr + 1).unwrap_or(0);
            let t = lo as u16 | ((hi as u16) << 8);
            render_sub_tile(&cpu.mem.vram, &cpu.mem.cgram, t, sx, sy, &mut pixels, 16);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([16, 16], &pixels)
}

pub(super) fn render_bg_block_image(block_id: u8, cpu: &mut Cpu) -> egui::ColorImage {
    let mut pixels = vec![0u8; 16 * 16 * 4];
    let map16_bg = cpu.mem.cart.resolve("Map16BGTiles").unwrap_or(0);
    let block_ptr = map16_bg + block_id as u32 * 8;
    let sub_offsets = [(0u32, 0u32), (0, 8), (8, 0), (8, 8)];
    for (sub_i, (sx, sy)) in sub_offsets.into_iter().enumerate() {
        let tile_word_addr = block_ptr + (sub_i as u32) * 2;
        let lo = cpu.mem.cart.read(tile_word_addr).unwrap_or(0);
        let hi = cpu.mem.cart.read(tile_word_addr + 1).unwrap_or(0);
        let t = lo as u16 | ((hi as u16) << 8);
        render_sub_tile(&cpu.mem.vram, &cpu.mem.cgram, t, sx, sy, &mut pixels, 16);
    }
    egui::ColorImage::from_rgba_unmultiplied([16, 16], &pixels)
}

pub(super) fn render_sprite_preview_image(sprite_tiles: &[SpriteOamTile], cpu: &mut Cpu) -> egui::ColorImage {
    const PREVIEW_SIZE: usize = 48;

    if sprite_tiles.is_empty() {
        return sprite_preview_fallback_image(PREVIEW_SIZE);
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for tile in sprite_tiles {
        let tile_size = if tile.is_16x16 { 16 } else { 8 };
        min_x = min_x.min(tile.dx);
        min_y = min_y.min(tile.dy);
        max_x = max_x.max(tile.dx + tile_size);
        max_y = max_y.max(tile.dy + tile_size);
    }

    let sprite_w = (max_x - min_x).max(1) as usize;
    let sprite_h = (max_y - min_y).max(1) as usize;
    let origin_x = ((PREVIEW_SIZE.saturating_sub(sprite_w)) / 2) as i32 - min_x;
    let origin_y = ((PREVIEW_SIZE.saturating_sub(sprite_h)) / 2) as i32 - min_y;

    let mut pixels = vec![0u8; PREVIEW_SIZE * PREVIEW_SIZE * 4];
    for tile in sprite_tiles {
        let dx = origin_x + tile.dx;
        let dy = origin_y + tile.dy;
        let t = tile.tile_word;

        if tile.is_16x16 {
            let (xn, xf) = if t & 0x4000 == 0 { (0u32, 8u32) } else { (8, 0) };
            let (yn, yf) = if t & 0x8000 == 0 { (0u32, 8u32) } else { (8, 0) };
            let attr = t & 0xFE00;
            let base = t & 0x01FF;
            render_signed_sprite_sub_tile(
                &cpu.mem.vram,
                &cpu.mem.cgram,
                attr | (base & 0x1FF),
                dx + xn as i32,
                dy + yn as i32,
                &mut pixels,
                PREVIEW_SIZE,
            );
            render_signed_sprite_sub_tile(
                &cpu.mem.vram,
                &cpu.mem.cgram,
                attr | ((base + 1) & 0x1FF),
                dx + xf as i32,
                dy + yn as i32,
                &mut pixels,
                PREVIEW_SIZE,
            );
            render_signed_sprite_sub_tile(
                &cpu.mem.vram,
                &cpu.mem.cgram,
                attr | ((base + 16) & 0x1FF),
                dx + xn as i32,
                dy + yf as i32,
                &mut pixels,
                PREVIEW_SIZE,
            );
            render_signed_sprite_sub_tile(
                &cpu.mem.vram,
                &cpu.mem.cgram,
                attr | ((base + 17) & 0x1FF),
                dx + xf as i32,
                dy + yf as i32,
                &mut pixels,
                PREVIEW_SIZE,
            );
        } else {
            render_signed_sprite_sub_tile(&cpu.mem.vram, &cpu.mem.cgram, t, dx, dy, &mut pixels, PREVIEW_SIZE);
        }
    }

    if pixels.chunks_exact(4).all(|px| px[3] == 0) {
        sprite_preview_fallback_image(PREVIEW_SIZE)
    } else {
        egui::ColorImage::from_rgba_unmultiplied([PREVIEW_SIZE, PREVIEW_SIZE], &pixels)
    }
}

fn render_signed_sprite_sub_tile(vram: &[u8], cgram: &[u8], t: u16, x0: i32, y0: i32, pixels: &mut [u8], stride: usize) {
    let tile_num = ((t & 0x01FF) as usize) + 0x600;
    let pal = (((t >> 9) & 0x7) as usize) + 8;
    let flip_x = (t & 0x4000) != 0;
    let flip_y = (t & 0x8000) != 0;

    let tile_base = tile_num * 32;
    for ty in 0..8i32 {
        for tx in 0..8i32 {
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

            let pal_idx = pal * 16 + color_idx;
            let off_color = pal_idx * 2;
            if off_color + 1 >= cgram.len() {
                continue;
            }
            let lo = cgram[off_color] as u16;
            let hi = cgram[off_color + 1] as u16;
            let rgb = lo | (hi << 8);

            let r = ((rgb & 0x1F) << 3) as u8;
            let g = (((rgb >> 5) & 0x1F) << 3) as u8;
            let b = (((rgb >> 10) & 0x1F) << 3) as u8;

            let px_abs = x0 + tx;
            let py_abs = y0 + ty;
            if px_abs < 0 || py_abs < 0 {
                continue;
            }
            let off = ((py_abs as usize) * stride + px_abs as usize) * 4;
            if off + 3 < pixels.len() {
                pixels[off] = r;
                pixels[off + 1] = g;
                pixels[off + 2] = b;
                pixels[off + 3] = 255;
            }
        }
    }
}

fn sprite_preview_fallback_image(size: usize) -> egui::ColorImage {
    let mut pixels = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let off = (y * size + x) * 4;
            let border = x < 2 || y < 2 || x >= size - 2 || y >= size - 2;
            let accent = x == y || x + y == size - 1;
            let shade = if border {
                [220, 170, 40, 255]
            } else if accent {
                [120, 120, 120, 255]
            } else if ((x / 6) + (y / 6)) % 2 == 0 {
                [52, 52, 52, 255]
            } else {
                [82, 82, 82, 255]
            };
            pixels[off..off + 4].copy_from_slice(&shade);
        }
    }
    egui::ColorImage::from_rgba_unmultiplied([size, size], &pixels)
}
